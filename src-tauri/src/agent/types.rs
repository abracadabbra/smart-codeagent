//! AgentLoop 配置 / 结果类型。
//!
//! 借 Kivio `chat/agent/types.rs:37-107` 的 `AgentRunConfig` / `AgentRunResult`，
//! 砍掉了 assistant / provider 等 Kivio 专属字段（Phase 2 用 env 配 + 单 provider）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::tools::{AskUserResponseResult, ToolCallRecord};

/// Phase 2 配置：env 注入的全局单实例。
#[derive(Debug, Clone)]
pub struct AgentRunConfig {
    pub model: String,
    pub max_tokens: u32,
    pub max_tool_rounds: u32,
    pub system_prompt: String,
    /// 每个 round 内允许的并行 tool 上限（Kivio 是 12，Phase 2 调到 8）
    pub max_parallel_tool_calls_per_round: usize,
    /// 上下文窗口 token 上限（输入消息 + system prompt）。
    /// 超出时从最旧非 system 消息开始丢弃。
    pub context_window_tokens: u32,
}

impl Default for AgentRunConfig {
    fn default() -> Self {
        Self::from_settings(&crate::settings::ProviderConfig::default())
    }
}

impl AgentRunConfig {
    /// 从 settings.json 中的 ProviderConfig 构造运行配置。
    pub fn from_settings(provider: &crate::settings::ProviderConfig) -> Self {
        Self {
            model: provider.model.clone(),
            max_tokens: provider.max_tokens,
            max_tool_rounds: 8,
            system_prompt: "You are Smart CodeAgent, an AI coding assistant. \
                Be concise and helpful. \
                Use the provided tools (read_file, write_file, edit_file, run_command, \
                bash_output, kill_background, glob_files, search_files, list_dir, ask_user) \
                to inspect and modify the project. Always prefer read_file over write_file \
                when you only need to view content. \
                IMPORTANT: When you want to call a tool, use the tool_use mechanism directly. \
                NEVER output tool calls as JSON text — the system will not parse text-form \
                tool calls. Prefer relative paths over absolute paths for file operations \
                and shell commands (e.g. use 'rm -rf ./test' not 'rm -rf /Users/.../test')."
                .into(),
            max_parallel_tool_calls_per_round: 8,
            context_window_tokens: provider.context_window_tokens,
        }
    }
}

/// 单次 LLM 调用产生的工具调用草稿（用于追踪 SSE 流中累积的 tool_use）。
///
/// 借 Kivio `ToolCallDraft` 形态。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    /// 累积的 JSON 参数（input_json_delta 拼接后 parse）
    pub input: Value,
    /// 累积的 raw 字符串（前端 syntax highlight）
    pub input_raw: String,
}

/// LLM 单轮响应（流式累积的最终产物）。
#[derive(Debug, Clone, Default)]
pub struct RoundResponse {
    pub text: String,
    pub tool_uses: Vec<ToolUseBlock>,
    pub stop_reason: Option<String>,
}

/// 一轮跑完的最终结果（多个 round 累积）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunResult {
    pub final_text: String,
    pub tool_records: Vec<ToolCallRecord>,
    pub ask_user_response: Option<AskUserResponseResult>,
    pub rounds: u32,
}

/// 单次工具调用解析 + 派发上下文。
///
/// 借 Kivio `ToolExecutionContext`，砍掉 sub-agent / depth。
#[derive(Debug, Clone)]
pub struct ToolDispatchContext {
    pub run_id: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub round: u32,
    pub generation: u64,
}
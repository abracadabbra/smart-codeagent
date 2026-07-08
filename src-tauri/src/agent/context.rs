//! Context 窗口管理：在发送给 LLM 前裁剪历史消息，避免超出模型上下文限制。
//!
//! Phase 5.1 实现一个保守的字符数估算器（1 token ≈ 3.5 字符），不引入 tiktoken-rs
//! 依赖。未来若需要精确计数，可替换 `estimate_tokens`。
//!
//! 策略：
//! - system prompt 始终保留
//! - 从最早一条非 system 消息开始丢弃
//! - 丢弃 assistant tool_call 消息时，同步丢弃其后的 tool 结果消息
//! - 保留最近的用户输入和 assistant 回复

use super::Message;

/// 默认上下文窗口大小（token 数）。
///
/// 对 deepseek-v4-flash 等 64K 窗口模型，留出 8K 给输出和工具定义。
pub const DEFAULT_CONTEXT_WINDOW_TOKENS: u32 = 56_000;

/// 单条消息基础开销（system/role 等），经验值 4 tokens。
const MESSAGE_OVERHEAD_TOKENS: usize = 4;

/// 保守估算：每 token 约 3.5 个字符（中文更费 token）。
const CHARS_PER_TOKEN: usize = 3;

/// 估算单条消息的 token 数。
pub fn estimate_tokens(msg: &Message) -> usize {
    let content_len = msg.content.as_ref().map(|s| s.len()).unwrap_or(0);

    let tool_calls_len = msg
        .tool_calls
        .as_ref()
        .map(|ts| {
            ts.iter()
                .map(|t| t.function.name.len() + t.function.arguments.len())
                .sum::<usize>()
        })
        .unwrap_or(0);

    let tool_call_id_len = msg.tool_call_id.as_ref().map(|s| s.len()).unwrap_or(0);

    let total_chars = content_len + tool_calls_len + tool_call_id_len;
    MESSAGE_OVERHEAD_TOKENS + total_chars.div_ceil(CHARS_PER_TOKEN)
}

/// 根据最大 token 限制裁剪消息列表。
///
/// - `messages`：原始历史消息（已包含 system prompt）
/// - `max_tokens`：允许的最大上下文 token 数
/// - 返回：裁剪后的消息列表，system prompt 始终在列表开头
///
/// 丢弃规则：
/// 1. 计算总 token 数，如果未超限直接返回
/// 2. 从索引 1 开始（保留 system），向后扫描
/// 3. 遇到 assistant 的 tool_call 消息时，标记其 id
/// 4. 后续 tool 消息的 tool_call_id 如果在丢弃集合中，也丢弃
/// 5. 直到剩余消息总 token <= max_tokens
pub fn trim_messages(messages: &[Message], max_tokens: u32) -> Vec<Message> {
    if messages.is_empty() {
        return Vec::new();
    }

    let max = max_tokens as usize;
    let total: usize = messages.iter().map(estimate_tokens).sum();
    if total <= max {
        return messages.to_vec();
    }

    // 第一条必须是 system；如果不是，从第一条开始保留。
    let has_system = messages[0].role == "system";
    let start_idx = if has_system { 1 } else { 0 };

    // 计算需要丢弃多少 token。
    let mut to_drop: usize = total - max;
    let mut dropped_tool_call_ids = std::collections::HashSet::new();
    let mut drop_indices = std::collections::HashSet::new();

    for i in start_idx..messages.len() {
        if to_drop == 0 {
            break;
        }

        let msg = &messages[i];

        // 如果是 tool 结果消息，检查其对应的 assistant tool_call 是否已被丢弃。
        if msg.role == "tool" {
            if let Some(id) = &msg.tool_call_id {
                if dropped_tool_call_ids.contains(id) {
                    drop_indices.insert(i);
                    to_drop = to_drop.saturating_sub(estimate_tokens(msg));
                    continue;
                }
            }
        }

        // 普通消息：直接标记丢弃，如果它是 assistant tool_call 则记录 id。
        drop_indices.insert(i);
        to_drop = to_drop.saturating_sub(estimate_tokens(msg));

        if let Some(tool_calls) = &msg.tool_calls {
            for t in tool_calls {
                dropped_tool_call_ids.insert(t.id.clone());
            }
        }
    }

    messages
        .iter()
        .enumerate()
        .filter(|(i, _)| !drop_indices.contains(i))
        .map(|(_, m)| m.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn system(content: &str) -> Message {
        msg("system", content)
    }

    fn user(content: &str) -> Message {
        msg("user", content)
    }

    fn assistant(content: &str) -> Message {
        msg("assistant", content)
    }

    fn assistant_with_tool(id: &str, name: &str, args: &str) -> Message {
        Message {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(vec![super::super::OpenAiToolCall {
                id: id.into(),
                call_type: "function".into(),
                function: super::super::OpenAiFunction {
                    name: name.into(),
                    arguments: args.into(),
                },
            }]),
            tool_call_id: None,
        }
    }

    fn tool_result(id: &str, content: &str) -> Message {
        Message {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(id.into()),
        }
    }

    #[test]
    fn no_trim_when_under_limit() {
        let messages = vec![system("sys"), user("hi"), assistant("hello")];
        let trimmed = trim_messages(&messages, 1_000_000);
        assert_eq!(trimmed.len(), 3);
    }

    #[test]
    fn keeps_system_when_trimming() {
        let messages = vec![
            system("sys"),
            user("old message 1"),
            user("old message 2"),
            user("recent question"),
            assistant("recent answer"),
        ];
        let trimmed = trim_messages(&messages, 30);
        assert_eq!(trimmed[0].role, "system");
        assert!(
            trimmed.iter().any(|m| m.content.as_deref() == Some("recent question")),
            "应保留最近用户消息"
        );
        assert!(
            trimmed.iter().any(|m| m.content.as_deref() == Some("recent answer")),
            "应保留最近 assistant 回复"
        );
        assert!(
            !trimmed.iter().any(|m| m.content.as_deref() == Some("old message 1")),
            "应丢弃最旧用户消息"
        );
    }

    #[test]
    fn drops_tool_results_with_orphaned_tool_call() {
        let messages = vec![
            system("sys"),
            user("read file"),
            assistant_with_tool("call_1", "read_file", "{\"path\":\"a.txt\"}"),
            tool_result("call_1", "content of a.txt is long long long"),
            user("next question"),
            assistant("answer"),
        ];

        // 限制很小，必须丢弃 assistant_with_tool 那轮。
        let trimmed = trim_messages(&messages, 30);

        assert_eq!(trimmed[0].role, "system");
        assert!(
            !trimmed.iter().any(|m| m.tool_call_id.as_deref() == Some("call_1")),
            "应同步丢弃对应 tool 结果"
        );
        assert!(
            trimmed.iter().any(|m| m.content.as_deref() == Some("next question")),
            "应保留最近用户消息"
        );
    }
}

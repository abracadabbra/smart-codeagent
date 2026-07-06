//! Tool round 派发：单 round 内并行执行多个 tool_use。
//!
//! 借 Kivio `rounds.rs:36` 的 `run_tool_round` 形态，简化掉 cancelled / 审批递归等。
//!
//! Phase 2 简化：
//! - 不并行（先用串行，更稳）；下一轮再 join_all
//! - approval gate 走 execute_tool_call 内部
//! - 失败的 tool 也产一个 ToolResultBlock（Error kind），不阻断后续
//!
//! Phase 3.1：新增 `mcp__` 前缀路由分支，MCP tool 走 `dispatch_mcp`。

use std::sync::Arc;

use crate::agent::host::AgentHost;
use crate::agent::tools::{
    ChatToolDefinition, ToolCallRecord, ToolCallStatus, ToolContext, ToolOutput, ToolRegistry,
};
use crate::agent::types::ToolUseBlock;
use crate::agent::host_impl::emit_tool_rejected;
use crate::mcp::{parse_mcp_name, McpManager};
use serde::Serialize;
use tauri::AppHandle;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolResultKind {
    Success { content: String },
    Error { message: String },
    Denied { reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    pub kind: ToolResultKind,
}

/// 派发一轮 tool_use：依次执行每个 tool，收集结果。
///
/// 借 Kivio `rounds.rs:36 run_tool_round`，砍掉了 multi-round 嵌套。
///
/// `mcp_manager`：Phase 3.1 MCP tool 路由用。`None` 时 MCP tool 调用返回 Error。
/// `tool_defs`：合并后的 native + mcp tool 定义，用于 MCP tool 的 sensitive 查找。
pub async fn dispatch_round(
    tools: &ToolRegistry,
    mcp_manager: Option<&Arc<McpManager>>,
    tool_defs: &[ChatToolDefinition],
    host: &Arc<dyn AgentHost>,
    ctx: &ToolContext,
    tool_uses: &[ToolUseBlock],
) -> Vec<ToolResultBlock> {
    info!(
        "dispatch_round start: round={}, tool_use_count={}",
        ctx.round,
        tool_uses.len()
    );
    let mut results = Vec::with_capacity(tool_uses.len());

    for (i, tool_use) in tool_uses.iter().enumerate() {
        info!(
            "dispatch[{}]: id={}, name={}, input={}",
            i, tool_use.id, tool_use.name, tool_use.input
        );
        let result = dispatch_single(tools, mcp_manager, tool_defs, host, ctx, tool_use).await;
        info!(
            "dispatch[{}] done: id={}, result_kind={}",
            i,
            result.tool_use_id,
            match &result.kind {
                ToolResultKind::Success { content } => {
                    format!("Success(len={})", content.len())
                }
                ToolResultKind::Error { message } => format!("Error({})", message),
                ToolResultKind::Denied { reason } => format!("Denied({})", reason),
            }
        );
        results.push(result);
    }

    info!("dispatch_round end: round={}, results={}", ctx.round, results.len());
    results
}

async fn dispatch_single(
    tools: &ToolRegistry,
    mcp_manager: Option<&Arc<McpManager>>,
    tool_defs: &[ChatToolDefinition],
    host: &Arc<dyn AgentHost>,
    ctx: &ToolContext,
    tool_use: &ToolUseBlock,
) -> ToolResultBlock {
    // Phase 3.1: MCP tool 命名空间路由
    if tool_use.name.starts_with("mcp__") {
        return dispatch_mcp(mcp_manager, tool_defs, host, ctx, tool_use).await;
    }

    let tool_call_id = tool_use.id.clone();
    let tool_name = tool_use.name.clone();

    // 1. 查找工具
    let tool = match tools.by_name(&tool_name) {
        Some(t) => {
            debug!(
                "tool lookup: name={} found, sensitive={}, destructive={}",
                tool_name,
                t.is_sensitive(),
                t.is_destructive()
            );
            t
        }
        None => {
            warn!("tool lookup failed: unknown tool {}", tool_name);
            let reason = format!("unknown tool: {tool_name}");
            emit_rejected(host, ctx, &tool_call_id, &tool_name, &reason);
            return ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Denied { reason },
            };
        }
    };

    // 2. approval gate（sensitive 或 destructive 都走）
    if tool.is_sensitive() || tool.is_destructive() {
        info!(
            "tool {} requires approval (sensitive={}, destructive={})",
            tool_name,
            tool.is_sensitive(),
            tool.is_destructive()
        );
        let mut record = ToolCallRecord {
            id: tool_call_id.clone(),
            name: tool_name.clone(),
            source: "native".into(),
            server_id: None,
            arguments: serde_json::to_string(&tool_use.input).unwrap_or_default(),
            status: ToolCallStatus::Pending,
            result_preview: None,
            error: None,
            duration_ms: None,
            started_at: Some(chrono::Utc::now().timestamp()),
            completed_at: None,
            round: ctx.round,
            sensitive: true,
            artifacts: vec![],
            structured_content: None,
        };
        host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

        let mut sub_ctx = ctx.clone();
        sub_ctx.tool_call_id = tool_call_id.clone();
        info!("requesting approval for tool {} (id={})", tool_name, tool_call_id);
        let approval_started = std::time::Instant::now();
        let approved = host.request_tool_approval(&sub_ctx, &record).await;
        info!(
            "approval response for tool {}: approved={}, waited_ms={}",
            tool_name,
            approved,
            approval_started.elapsed().as_millis()
        );

        if !approved {
            warn!("tool {} approval denied by user", tool_name);
            record.status = ToolCallStatus::Cancelled;
            record.error = Some("user denied approval".into());
            record.completed_at = Some(chrono::Utc::now().timestamp());
            host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

            return ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Denied {
                    reason: "user denied approval".into(),
                },
            };
        }
    }

    // 3. 实际执行
    let mut sub_ctx = ctx.clone();
    sub_ctx.tool_call_id = tool_call_id.clone();
    let args = tool_use.input.clone();
    let tool_name_for_record = tool_name.clone();

    let started = chrono::Utc::now().timestamp();
    info!(
        "executing tool {}: id={}, args={}",
        tool_name_for_record, tool_call_id, args
    );
    let execute_result = tool.execute(args, &sub_ctx).await;
    let duration_ms = (chrono::Utc::now().timestamp() - started).max(0) as u64;
    info!(
        "tool {} executed: duration_ms={}, result_ok={}",
        tool_name_for_record,
        duration_ms,
        execute_result.is_ok()
    );

    match execute_result {
        Ok(output) => {
            debug!(
                "tool {} output: content_len={}, structured={:?}, artifacts={}",
                tool_name_for_record,
                output.content.len(),
                output.structured.is_some(),
                output.artifacts.len()
            );
            // emit Success 记录
            let preview = if output.content.len() > 200 {
                format!("{}…", &output.content[..200])
            } else {
                output.content.clone()
            };
            let record = ToolCallRecord {
                id: tool_call_id.clone(),
                name: tool_name_for_record,
                source: "native".into(),
                server_id: None,
                arguments: serde_json::to_string(&tool_use.input).unwrap_or_default(),
                status: ToolCallStatus::Success,
                result_preview: Some(preview),
                error: None,
                duration_ms: Some(duration_ms),
                started_at: Some(started),
                completed_at: Some(chrono::Utc::now().timestamp()),
                round: ctx.round,
                sensitive: tool.is_sensitive() || tool.is_destructive(),
                artifacts: output.artifacts.clone(),
                structured_content: output.structured,
            };
            host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

            ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Success {
                    content: output.content,
                },
            }
        }
        Err(e) => {
            warn!(
                "tool {} failed: error={:?}",
                tool_name_for_record, e
            );
            // 区分：Denied 是 permission 类，Error 是执行失败类
            let (kind_label, msg) = match &e {
                crate::agent::tools::ToolError::Denied(reason) => {
                    emit_rejected(host, ctx, &tool_call_id, &tool_name, reason);
                    ("denied", reason.clone())
                }
                _ => ("error", format!("{e}")),
            };
            let record = ToolCallRecord {
                id: tool_call_id.clone(),
                name: tool_name_for_record,
                source: "native".into(),
                server_id: None,
                arguments: serde_json::to_string(&tool_use.input).unwrap_or_default(),
                status: if kind_label == "denied" {
                    ToolCallStatus::Cancelled
                } else {
                    ToolCallStatus::Error
                },
                result_preview: None,
                error: Some(msg.clone()),
                duration_ms: Some(duration_ms),
                started_at: Some(started),
                completed_at: Some(chrono::Utc::now().timestamp()),
                round: ctx.round,
                sensitive: tool.is_sensitive() || tool.is_destructive(),
                artifacts: vec![],
                structured_content: None,
            };
            host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

            let kind = if kind_label == "denied" {
                ToolResultKind::Denied { reason: msg }
            } else {
                ToolResultKind::Error { message: msg }
            };
            ToolResultBlock {
                tool_use_id: tool_call_id,
                kind,
            }
        }
    }
}

/// 派发 MCP tool 调用：解析命名空间 → approval gate（若 sensitive）→ mcp_manager.call_tool。
///
/// 设计参照 [design.md §2.3](../../.trellis/tasks/07-05-phase3.1-mcp-integration/design.md)。
/// - sensitive 查找：从 `tool_defs` 中按 name 匹配；未找到则默认 sensitive=true（保守）。
/// - MCP `isError=true`：仍算 Success（server 自报 tool-level 错误，content 是错误描述），
///   structured_content 设为 `{"isError":true}` 让 LLM 知晓。
/// - transport 错误（超时/死连接）：返回 Error kind，content = `MCP tool error: {e}`。
async fn dispatch_mcp(
    mcp_manager: Option<&Arc<McpManager>>,
    tool_defs: &[ChatToolDefinition],
    host: &Arc<dyn AgentHost>,
    ctx: &ToolContext,
    tool_use: &ToolUseBlock,
) -> ToolResultBlock {
    let tool_call_id = tool_use.id.clone();
    let tool_name = tool_use.name.clone();

    let mcp_manager = match mcp_manager {
        Some(m) => m,
        None => {
            return ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Error {
                    message: "MCP manager not available".into(),
                },
            };
        }
    };

    let (server_id, mcp_tool_name) = match parse_mcp_name(&tool_name) {
        Some(parsed) => parsed,
        None => {
            return ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Error {
                    message: format!("invalid MCP tool name: {tool_name}"),
                },
            };
        }
    };

    // sensitive 查找：从合并后的 tool_defs 中按 name 匹配。
    // 未找到（理论上不该发生，因为 prepare 阶段已合并）→ 保守视为 sensitive。
    let sensitive = tool_defs
        .iter()
        .find(|d| d.name == tool_name)
        .map(|d| d.sensitive)
        .unwrap_or(true);

    // approval gate（与 native destructive 同路径）
    if sensitive {
        info!(
            "MCP tool {} requires approval (sensitive={})",
            tool_name, sensitive
        );
        let mut record = ToolCallRecord {
            id: tool_call_id.clone(),
            name: tool_name.clone(),
            source: "mcp".into(),
            server_id: Some(server_id.clone()),
            arguments: serde_json::to_string(&tool_use.input).unwrap_or_default(),
            status: ToolCallStatus::Pending,
            result_preview: None,
            error: None,
            duration_ms: None,
            started_at: Some(chrono::Utc::now().timestamp()),
            completed_at: None,
            round: ctx.round,
            sensitive: true,
            artifacts: vec![],
            structured_content: None,
        };
        host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

        let mut sub_ctx = ctx.clone();
        sub_ctx.tool_call_id = tool_call_id.clone();
        let approved = host.request_tool_approval(&sub_ctx, &record).await;

        if !approved {
            warn!("MCP tool {} approval denied by user", tool_name);
            record.status = ToolCallStatus::Cancelled;
            record.error = Some("user denied approval".into());
            record.completed_at = Some(chrono::Utc::now().timestamp());
            host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);
            return ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Denied {
                    reason: "user denied approval".into(),
                },
            };
        }
    }

    // 实际执行
    let started = chrono::Utc::now().timestamp();
    info!(
        "executing MCP tool {}: server={}, tool={}, args={}",
        tool_name, server_id, mcp_tool_name, tool_use.input
    );
    let result = mcp_manager
        .call_tool(&server_id, &mcp_tool_name, tool_use.input.clone())
        .await;
    let duration_ms = (chrono::Utc::now().timestamp() - started).max(0) as u64;

    match result {
        Ok(res) => {
            // MCP isError=true → 仍算 Success（server 自报 tool-level 错误）
            let structured = if res.is_error {
                Some(serde_json::json!({ "isError": true }))
            } else {
                res.structured_content.clone()
            };
            let preview = if res.content.len() > 200 {
                format!("{}…", &res.content[..200])
            } else {
                res.content.clone()
            };
            let record = ToolCallRecord {
                id: tool_call_id.clone(),
                name: tool_name.clone(),
                source: "mcp".into(),
                server_id: Some(server_id),
                arguments: serde_json::to_string(&tool_use.input).unwrap_or_default(),
                status: ToolCallStatus::Success,
                result_preview: Some(preview),
                error: None,
                duration_ms: Some(duration_ms),
                started_at: Some(started),
                completed_at: Some(chrono::Utc::now().timestamp()),
                round: ctx.round,
                sensitive,
                artifacts: res.artifacts.clone(),
                structured_content: structured,
            };
            host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

            ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Success {
                    content: res.content,
                },
            }
        }
        Err(e) => {
            let msg = format!("MCP tool error: {e}");
            warn!("MCP tool {} failed: {}", tool_name, msg);
            let record = ToolCallRecord {
                id: tool_call_id.clone(),
                name: tool_name.clone(),
                source: "mcp".into(),
                server_id: Some(server_id),
                arguments: serde_json::to_string(&tool_use.input).unwrap_or_default(),
                status: ToolCallStatus::Error,
                result_preview: None,
                error: Some(msg.clone()),
                duration_ms: Some(duration_ms),
                started_at: Some(started),
                completed_at: Some(chrono::Utc::now().timestamp()),
                round: ctx.round,
                sensitive,
                artifacts: vec![],
                structured_content: None,
            };
            host.emit_tool_record(&ctx.conversation_id, &ctx.run_id, &ctx.message_id, &record);

            ToolResultBlock {
                tool_use_id: tool_call_id,
                kind: ToolResultKind::Error { message: msg },
            }
        }
    }
}

fn emit_rejected(
    host: &Arc<dyn AgentHost>,
    ctx: &ToolContext,
    tool_call_id: &str,
    tool_name: &str,
    reason: &str,
) {
    // 这里没法拿到 AppHandle；host 自己负责 emit tool_rejected。
    // Phase 2 简化：host trait 不直接 emit tool_rejected（避免再加方法），
    // 而是把 reason 推回 tool_result block 让前端从 tool_record 看到。
    // （UI 在 agent:tool_record 事件里如果 status=Cancelled + error 包含
    // "permission denied"/"blocked by safety policy"，显示红色卡片即可。）
    let _ = host;
    let _ = ctx;
    let _ = tool_call_id;
    let _ = tool_name;
    let _ = reason;
    // 留给前端用 status 字段判定
}

/// 给 main.rs / commands.rs 用的入口：emit `agent:tool_rejected` 事件。
/// 这是 host trait 之外的直接 emit，因为 tool_rejected 是 loop-level 信号
/// （不是某个 tool 的记录）。
///
/// Phase 3.2：加 `conversation_id` 参数。
pub fn emit_rejected_direct(
    app: &AppHandle,
    conversation_id: &str,
    run_id: &str,
    message_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    reason: &str,
) {
    emit_tool_rejected(app, conversation_id, run_id, message_id, tool_call_id, tool_name, reason);
}

// 反引用 ToolOutput 防止 unused warning
#[allow(dead_code)]
fn _unused(_: &ToolOutput) {}
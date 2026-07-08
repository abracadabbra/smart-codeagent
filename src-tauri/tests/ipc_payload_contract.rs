//! IPC payload 序列化契约测试。
//!
//! Tauri 2 IPC 契约（Phase 1.2 决策）：
//! - Rust 端通过 `#[serde(rename_all = "camelCase")]` 输出 camelCase JSON
//! - 前端 invoke payload 是扁平对象，key 直接对应 Rust 命令参数名（camelCase）
//! - 前端 `useAgentEvents.ts` 事件 payload 类型已是 camelCase，刚好对得上
//!
//! 这些测试钉死序列化契约，防止后续重构把命名风格改回 snake_case
//! 而前端没同步而失联。
//!
//! Phase 1 (5 个)：用镜像 struct 自带 `#[serde(rename_all = "camelCase")]`
//! Phase 2 (8 个新增)：直接用 `smart_codeagent_lib::ipc::events` 真实类型，
//!   这样如果有人改了 events.rs 里的 serde 属性，测试会立刻失败。
//! Phase 3.2 (15 个更新 + 4 个新增)：
//!   - 所有 payload 加 `conversationId` 字段（前端按 conv 路由事件）
//!   - `SendMessageArgs` 改签名：`conversation_id` + `run_id` 替代 `assistant_id`
//!   - `ApproveToolArgs` / `AnswerAskUserArgs` 加 `conversation_id`
//!   - 新增 4 个 session 事件 payload 测试（created/updated/deleted/state）

use serde::Serialize;
use smart_codeagent_lib::agent::tools::{
    AskUserOption, AskUserPromptPayload, AskUserQuestion, ToolCallRecord, ToolCallStatus,
};
use smart_codeagent_lib::ipc::events::{
    AgentApprovalRequestPayload, AgentAskUserPromptPayload, AgentPartialAssistantPayload,
    AgentStreamDeltaPayload, AgentStreamDonePayload, AgentToolRecordPayload,
    AgentToolRejectedPayload, SessionCreatedPayload, SessionDeletedPayload, SessionStatePayload,
    SessionUpdatedPayload,
};
use smart_codeagent_lib::session::types::Conversation;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentTokenPayload {
    pub conversation_id: String,
    pub msg_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatusPayload {
    pub conversation_id: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentErrorPayload {
    pub conversation_id: String,
    pub msg_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentDonePayload {
    pub conversation_id: String,
    pub msg_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageArgs {
    pub conversation_id: String,
    pub text: String,
    pub run_id: String,
}

#[test]
fn agent_token_payload_serializes_camel_case() {
    let p = AgentTokenPayload {
        conversation_id: "conv_abc".into(),
        msg_id: "asst-1".into(),
        text: "hi".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(
        json["conversationId"], "conv_abc",
        "前端 useAgentEvents.ts 按 conversationId 路由"
    );
    assert_eq!(
        json["msgId"], "asst-1",
        "前端 useAgentEvents.ts 依赖 msgId 字段名"
    );
    assert_eq!(json["text"], "hi");
    assert!(
        json.get("conversation_id").is_none(),
        "不能出现 snake_case 字段"
    );
    assert!(json.get("msg_id").is_none());
}

#[test]
fn agent_status_payload_serializes_camel_case() {
    let p = AgentStatusPayload {
        conversation_id: "conv_abc".into(),
        state: "Stream".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["state"], "Stream");
    assert!(json.get("conversation_id").is_none());
}

#[test]
fn agent_error_payload_serializes_camel_case() {
    let p = AgentErrorPayload {
        conversation_id: "conv_abc".into(),
        msg_id: "asst-1".into(),
        message: "401 unauthorized".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["msgId"], "asst-1");
    assert_eq!(json["message"], "401 unauthorized");
    assert!(json.get("conversation_id").is_none());
    assert!(json.get("msg_id").is_none());
}

#[test]
fn agent_done_payload_serializes_camel_case() {
    let p = AgentDonePayload {
        conversation_id: "conv_abc".into(),
        msg_id: "asst-1".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["msgId"], "asst-1");
    assert!(json.get("conversation_id").is_none());
}

#[test]
fn send_message_args_serializes_camel_case() {
    // Tauri 2 契约：invoke payload 顶层 key 直接映射 Rust 函数参数，
    // 必须 camelCase 才能匹配 conversation_id / run_id 函数参数。
    // Phase 3.2：send_message 签名改为 (conversation_id, text, run_id) —— 后端生成 message_id + generation
    let args = SendMessageArgs {
        conversation_id: "conv_abc".into(),
        text: "hi".into(),
        run_id: "run-1".into(),
    };
    let json = serde_json::to_value(&args).unwrap();
    assert_eq!(
        json["conversationId"], "conv_abc",
        "前端 invoke 用 conversationId"
    );
    assert_eq!(json["text"], "hi");
    assert_eq!(json["runId"], "run-1", "前端 invoke 用 runId");
    assert!(
        json.get("conversation_id").is_none(),
        "Tauri 2 不做 snake↔camel 转换"
    );
    assert!(json.get("run_id").is_none());
    // 旧字段 assistantId / generation 不应出现（后端生成）
    assert!(
        json.get("assistantId").is_none(),
        "assistantId 已由后端生成，前端不再传"
    );
    assert!(
        json.get("generation").is_none(),
        "generation 已由后端生成，前端不再传"
    );
}

// ============================================================================
// Phase 2 新增 8 个合约测试（用真实类型，捕获 events.rs serde 属性被改）
// Phase 3.2: 所有 payload 加 conversationId 字段
// ============================================================================

fn sample_tool_record() -> ToolCallRecord {
    ToolCallRecord {
        id: "tc_1".into(),
        name: "read_file".into(),
        source: "native".into(),
        server_id: None,
        arguments: r#"{"path":"foo.rs"}"#.into(),
        status: ToolCallStatus::Success,
        result_preview: Some("file contents".into()),
        error: None,
        duration_ms: Some(15),
        started_at: Some(1000),
        completed_at: Some(1015),
        round: 1,
        sensitive: false,
        artifacts: vec!["foo.rs".into()],
        structured_content: None,
    }
}

#[test]
fn stream_delta_payload_serializes_camel_case() {
    let p = AgentStreamDeltaPayload {
        conversation_id: "conv_abc".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        text: "hi".into(),
        reasoning_delta: None,
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["msgId"], "asst-1");
    assert_eq!(json["text"], "hi");
    assert!(
        json.get("reasoningDelta").is_none(),
        "reasoningDelta=None 必须 skip（skip_serializing_if）"
    );
    assert!(json.get("conversation_id").is_none());
    assert!(json.get("run_id").is_none());

    // 有 reasoning 时字段出现
    let p2 = AgentStreamDeltaPayload {
        reasoning_delta: Some("because".into()),
        ..p
    };
    let j2 = serde_json::to_value(&p2).unwrap();
    assert_eq!(j2["reasoningDelta"], "because");
}

#[test]
fn stream_done_payload_serializes_camel_case() {
    let p = AgentStreamDonePayload {
        conversation_id: "conv_abc".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        reason: "end_turn".into(),
        full_text: "hello world".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["msgId"], "asst-1");
    assert_eq!(json["reason"], "end_turn");
    assert_eq!(json["fullText"], "hello world");
    assert!(json.get("conversation_id").is_none());
    assert!(json.get("run_id").is_none());
}

#[test]
fn tool_record_payload_serializes_camel_case() {
    let p = AgentToolRecordPayload {
        conversation_id: "conv_abc".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        record: sample_tool_record(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["msgId"], "asst-1");
    // 嵌套 record 也要 camelCase
    assert_eq!(json["record"]["id"], "tc_1");
    assert_eq!(json["record"]["durationMs"], 15);
    assert_eq!(json["record"]["startedAt"], 1000);
    assert_eq!(json["record"]["resultPreview"], "file contents");
    assert!(
        json["record"].get("duration_ms").is_none(),
        "嵌套 snake_case 泄漏"
    );
    assert!(json.get("conversation_id").is_none());
}

#[test]
fn approval_request_payload_serializes_camel_case() {
    let p = AgentApprovalRequestPayload {
        conversation_id: "conv_abc".into(),
        approval_id: "appr-1".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        tool_call_id: "tc_1".into(),
        tool_name: "write_file".into(),
        arguments: r#"{"path":"foo"}"#.into(),
        sensitive: true,
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["approvalId"], "appr-1");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["msgId"], "asst-1");
    assert_eq!(json["toolCallId"], "tc_1");
    assert_eq!(json["toolName"], "write_file");
    assert_eq!(json["sensitive"], true);
    assert!(json.get("conversation_id").is_none());
    assert!(json.get("approval_id").is_none());
    assert!(json.get("tool_call_id").is_none());
}

#[test]
fn ask_user_prompt_payload_serializes_camel_case() {
    let prompt = AskUserPromptPayload {
        title: Some("Pick runtime".into()),
        questions: vec![AskUserQuestion {
            id: "q1".into(),
            prompt: "Which?".into(),
            options: vec![AskUserOption {
                id: "node".into(),
                label: "Node.js".into(),
                description: None,
            }],
            allow_multiple: false,
            allow_custom: true,
        }],
    };
    let p = AgentAskUserPromptPayload {
        conversation_id: "conv_abc".into(),
        ask_user_id: "ask-1".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        tool_call_id: "tc_1".into(),
        prompt,
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["askUserId"], "ask-1");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["toolCallId"], "tc_1");
    assert_eq!(json["prompt"]["title"], "Pick runtime");
    assert_eq!(json["prompt"]["questions"][0]["allowMultiple"], false);
    assert_eq!(json["prompt"]["questions"][0]["allowCustom"], true);
    assert!(json.get("conversation_id").is_none());
    assert!(json.get("ask_user_id").is_none());
}

#[test]
fn partial_assistant_payload_serializes_camel_case() {
    let p = AgentPartialAssistantPayload {
        conversation_id: "conv_abc".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        records: vec![sample_tool_record()],
        api_messages: vec![serde_json::json!({"role": "user", "content": "hi"})],
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["msgId"], "asst-1");
    assert!(json["records"].is_array());
    assert_eq!(json["records"][0]["id"], "tc_1");
    assert_eq!(json["apiMessages"][0]["role"], "user");
    assert!(json.get("api_messages").is_none(), "snake_case 字段泄漏");
    assert!(json.get("conversation_id").is_none());
}

#[test]
fn tool_rejected_payload_serializes_camel_case() {
    let p = AgentToolRejectedPayload {
        conversation_id: "conv_abc".into(),
        run_id: "run-1".into(),
        msg_id: "asst-1".into(),
        tool_call_id: "tc_1".into(),
        tool_name: "run_command".into(),
        reason: "command blocked by safety policy".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc");
    assert_eq!(json["runId"], "run-1");
    assert_eq!(json["msgId"], "asst-1");
    assert_eq!(json["toolCallId"], "tc_1");
    assert_eq!(json["toolName"], "run_command");
    assert_eq!(json["reason"], "command blocked by safety policy");
    assert!(json.get("tool_call_id").is_none());
    assert!(json.get("conversation_id").is_none());
}

/// ApproveToolArgs 在 commands.rs 里只有 Deserialize（前端→后端），
/// 这里测反方向：构造前端会发的 camelCase JSON，验证真实类型能反序列化。
/// Phase 3.2: ApproveToolArgs 加 conversation_id 字段。
#[test]
fn approve_tool_args_deserializes_camel_case() {
    use smart_codeagent_lib::ipc::commands::ApproveToolArgs;

    let json = serde_json::json!({
        "conversationId": "conv_abc",
        "approvalId": "appr-1",
        "allow": true,
    });
    let args: ApproveToolArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.conversation_id, "conv_abc");
    assert_eq!(args.approval_id, "appr-1");
    assert!(args.allow);

    // snake_case 必须 NOT 工作（前端不会发，但若后端被改成 snake_case only 会破）
    let snake = serde_json::json!({
        "conversation_id": "conv_abc",
        "approval_id": "x",
        "allow": false,
    });
    assert!(
        serde_json::from_value::<ApproveToolArgs>(snake).is_err(),
        "后端不应接受 snake_case（前端发的是 camelCase）"
    );
}

/// Phase 3.2: AnswerAskUserArgs 同样加 conversation_id 字段。
#[test]
fn answer_ask_user_args_deserializes_camel_case() {
    use smart_codeagent_lib::agent::tools::AskUserResponseResult;
    use smart_codeagent_lib::ipc::commands::AnswerAskUserArgs;

    let json = serde_json::json!({
        "conversationId": "conv_abc",
        "askUserId": "ask-1",
        "response": {
            "phase": "answered",
            "answers": {},
        },
    });
    let args: AnswerAskUserArgs = serde_json::from_value(json).unwrap();
    assert_eq!(args.conversation_id, "conv_abc");
    assert_eq!(args.ask_user_id, "ask-1");
    assert_eq!(args.response.phase, "answered");

    // snake_case 必须 NOT 工作
    let snake = serde_json::json!({
        "conversation_id": "conv_abc",
        "ask_user_id": "x",
        "response": {
            "phase": "answered",
            "answers": {},
        },
    });
    assert!(
        serde_json::from_value::<AnswerAskUserArgs>(snake).is_err(),
        "后端不应接受 snake_case（前端发的是 camelCase）"
    );

    // 反序列化的 AskUserResponseResult 也要能 round-trip
    let _resp: AskUserResponseResult = args.response;
}

// ============================================================================
// Phase 3.1 新增 2 个合约测试（MCP 相关）
// ============================================================================

#[test]
fn mcp_server_state_payload_serializes_camel_case() {
    use smart_codeagent_lib::mcp::{McpServerState, McpServerStatePayload};

    // Connected
    let p = McpServerStatePayload {
        server_id: "fs".into(),
        state: McpServerState::Connected,
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(
        json["serverId"], "fs",
        "前端 useAgentEvents.ts 依赖 serverId"
    );
    assert_eq!(json["state"]["kind"], "connected");
    assert!(json.get("server_id").is_none(), "snake_case 泄漏");

    // Error { message }
    let p_err = McpServerStatePayload {
        server_id: "db".into(),
        state: McpServerState::Error {
            message: "connection refused".into(),
        },
    };
    let j_err = serde_json::to_value(&p_err).unwrap();
    assert_eq!(j_err["serverId"], "db");
    assert_eq!(j_err["state"]["kind"], "error");
    assert_eq!(j_err["state"]["message"], "connection refused");

    // Connecting / Disconnected
    let p_conn = McpServerStatePayload {
        server_id: "x".into(),
        state: McpServerState::Connecting,
    };
    assert_eq!(
        serde_json::to_value(&p_conn).unwrap()["state"]["kind"],
        "connecting"
    );

    let p_disc = McpServerStatePayload {
        server_id: "x".into(),
        state: McpServerState::Disconnected,
    };
    assert_eq!(
        serde_json::to_value(&p_disc).unwrap()["state"]["kind"],
        "disconnected"
    );
}

#[test]
fn chat_mcp_server_round_trip_camel_case() {
    use smart_codeagent_lib::settings::ChatMcpServer;
    use std::collections::HashMap;

    // 构造前端会发的 camelCase JSON（settings.json 格式）
    let mut env = HashMap::new();
    env.insert("FOO".to_string(), "bar".to_string());
    let json = serde_json::json!({
        "id": "filesystem",
        "name": "Filesystem (/tmp)",
        "enabled": true,
        "transport": "stdio",
        "command": "npx",
        "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
        "env": { "FOO": "bar" },
        "enabledTools": ["read_file", "write_file"]
    });

    let server: ChatMcpServer = serde_json::from_value(json).unwrap();
    assert_eq!(server.id, "filesystem");
    assert_eq!(server.name, "Filesystem (/tmp)");
    assert!(server.enabled);
    assert_eq!(server.transport, "stdio");
    assert_eq!(server.command, "npx");
    assert_eq!(server.args.len(), 3);
    assert_eq!(server.env.get("FOO").map(|s| s.as_str()), Some("bar"));
    assert!(server.cwd.is_none());
    assert_eq!(server.enabled_tools, vec!["read_file", "write_file"]);

    // 反向序列化仍为 camelCase
    let out = serde_json::to_value(&server).unwrap();
    assert_eq!(out["id"], "filesystem");
    assert_eq!(
        out["enabledTools"],
        serde_json::json!(["read_file", "write_file"])
    );
    assert!(out.get("enabled_tools").is_none(), "snake_case 泄漏到 wire");

    // 默认值：enabled=true, transport="stdio"（缺省时）
    let minimal = serde_json::json!({
        "id": "x",
        "name": "X",
        "command": "echo"
    });
    let s: ChatMcpServer = serde_json::from_value(minimal).unwrap();
    assert!(s.enabled, "enabled 缺省必须为 true");
    assert_eq!(s.transport, "stdio", "transport 缺省必须为 stdio");
    assert!(s.enabled_tools.is_empty());
}

// ============================================================================
// Phase 3.2 新增 4 个合约测试（session 事件 payload）
// ============================================================================

fn sample_conversation() -> Conversation {
    Conversation {
        id: "conv_abc123".into(),
        title: "帮我看一下 auth 模块".into(),
        created_at: 1720000000000,
        updated_at: 1720000123000,
        pinned: false,
        message_count: 5,
    }
}

#[test]
fn session_created_payload_serializes_camel_case() {
    let p = SessionCreatedPayload {
        conversation: sample_conversation(),
    };
    let json = serde_json::to_value(&p).unwrap();
    // 顶层 conversation 对象
    assert_eq!(json["conversation"]["id"], "conv_abc123");
    assert_eq!(json["conversation"]["title"], "帮我看一下 auth 模块");
    assert_eq!(json["conversation"]["createdAt"], 1720000000000_i64);
    assert_eq!(json["conversation"]["updatedAt"], 1720000123000_i64);
    assert_eq!(json["conversation"]["pinned"], false);
    assert_eq!(json["conversation"]["messageCount"], 5);
    // 嵌套字段不能有 snake_case
    assert!(
        json["conversation"].get("created_at").is_none(),
        "嵌套 snake_case 泄漏"
    );
    assert!(
        json["conversation"].get("message_count").is_none(),
        "嵌套 snake_case 泄漏"
    );
}

#[test]
fn session_updated_payload_serializes_camel_case() {
    let p = SessionUpdatedPayload {
        conversation: sample_conversation(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversation"]["id"], "conv_abc123");
    assert_eq!(json["conversation"]["title"], "帮我看一下 auth 模块");
    assert_eq!(json["conversation"]["updatedAt"], 1720000123000_i64);
    assert_eq!(json["conversation"]["messageCount"], 5);
    assert!(json["conversation"].get("updated_at").is_none());
    assert!(json["conversation"].get("message_count").is_none());
}

#[test]
fn session_deleted_payload_serializes_camel_case() {
    let p = SessionDeletedPayload {
        conversation_id: "conv_abc123".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(
        json["conversationId"], "conv_abc123",
        "前端按 conversationId 路由 delete 事件"
    );
    assert!(json.get("conversation_id").is_none(), "snake_case 泄漏");
}

#[test]
fn session_state_payload_serializes_camel_case() {
    use smart_codeagent_lib::agent::AgentState;

    let p = SessionStatePayload {
        conversation_id: "conv_abc123".into(),
        state: AgentState::ToolLoop,
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["conversationId"], "conv_abc123");
    // AgentState 用 PascalCase 序列化（与 agent:status 事件一致）
    assert_eq!(json["state"], "ToolLoop");
    assert!(json.get("conversation_id").is_none(), "snake_case 泄漏");

    // Idle 状态
    let p_idle = SessionStatePayload {
        conversation_id: "conv_x".into(),
        state: AgentState::Idle,
    };
    assert_eq!(serde_json::to_value(&p_idle).unwrap()["state"], "Idle");
}

//! IPC payload 序列化契约测试。
//!
//! Tauri 2 IPC 契约（Phase 1.2 决策）：
//! - Rust 端通过 `#[serde(rename_all = "camelCase")]` 输出 camelCase JSON
//! - 前端 invoke payload 是扁平对象，key 直接对应 Rust 命令参数名（camelCase）
//! - 前端 `useAgentEvents.ts` 事件 payload 类型已是 camelCase，刚好对得上
//!
//! 这些测试钉死序列化契约，防止后续重构把命名风格改回 snake_case
//! 而前端没同步而失联。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentTokenPayload {
    pub msg_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatusPayload {
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentErrorPayload {
    pub msg_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentDonePayload {
    pub msg_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageArgs {
    pub text: String,
    pub assistant_id: String,
}

#[test]
fn agent_token_payload_serializes_camel_case() {
    let p = AgentTokenPayload {
        msg_id: "asst-1".into(),
        text: "hi".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["msgId"], "asst-1", "前端 useAgentEvents.ts 依赖 msgId 字段名");
    assert_eq!(json["text"], "hi");
    assert!(json.get("msg_id").is_none(), "不能出现 snake_case 字段");
}

#[test]
fn agent_status_payload_serializes_camel_case() {
    let p = AgentStatusPayload {
        state: "Stream".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["state"], "Stream");
}

#[test]
fn agent_error_payload_serializes_camel_case() {
    let p = AgentErrorPayload {
        msg_id: "asst-1".into(),
        message: "401 unauthorized".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["msgId"], "asst-1");
    assert_eq!(json["message"], "401 unauthorized");
    assert!(json.get("msg_id").is_none());
}

#[test]
fn agent_done_payload_serializes_camel_case() {
    let p = AgentDonePayload {
        msg_id: "asst-1".into(),
    };
    let json = serde_json::to_value(&p).unwrap();
    assert_eq!(json["msgId"], "asst-1");
}

#[test]
fn send_message_args_serializes_camel_case() {
    // Tauri 2 契约：invoke payload 顶层 key 直接映射 Rust 函数参数，
    // 必须 camelCase 才能匹配 assistant_id 函数参数。
    let args = SendMessageArgs {
        text: "hi".into(),
        assistant_id: "asst-1".into(),
    };
    let json = serde_json::to_value(&args).unwrap();
    assert_eq!(json["text"], "hi");
    assert_eq!(json["assistantId"], "asst-1", "前端 invoke 用 assistantId");
    assert!(json.get("assistant_id").is_none(), "Tauri 2 不做 snake↔camel 转换");
}
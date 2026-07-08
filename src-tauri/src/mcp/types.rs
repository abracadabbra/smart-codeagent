//! MCP 协议数据结构 + 转换函数。
//!
//! 纯数据层，无 IO、无并发。可独立单测。
//!
//! 参考实现：Kivio `mcp/types.rs` + `mcp/client.rs::parse_tool_result`，砍掉了：
//! - `output_schema`（Phase 3.1 不暴露给 LLM）
//! - `id` / `server_name` 字段（smart-codeagent 的 `ChatToolDefinition` 没这两个字段）
//! - 图片 artifact 的 size_bytes / path 元数据（只保留 data URL 字符串）
//! - `follow_up_user_messages`（vision 跟随消息，Phase 3.1 不需要）
//! - SSE 解析（Phase 3.1 只做 stdio，行级 JSON-RPC）

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::tools::ChatToolDefinition;
use crate::settings::ChatMcpServer;

/// MCP server 在 `tools/list` 中返回的单个 tool 描述。
///
/// 对应 MCP 协议 2025-06-18 的 `Tool` schema。
#[derive(Debug, Clone, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema 描述输入参数。可能为 `null`（server 没填）。
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
    /// MCP 协议的 tool annotations（readOnlyHint / destructiveHint / openWorldHint）。
    /// 用于决定是否走 approval 流。
    #[serde(default)]
    pub annotations: Option<Value>,
}

/// `tools/call` 的解析后结果。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpToolCallResult {
    /// 给 LLM 看的文本内容（concat 所有 `content[].text`）。
    pub content: String,
    /// MCP `isError` 标志。true 表示 server 自报错误（content 是错误描述）。
    #[serde(default)]
    pub is_error: bool,
    /// MCP `structuredContent`（如果 server 返回结构化数据）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<Value>,
    /// 图片等非文本产物的 data URL 列表（如 `data:image/png;base64,...`）。
    /// Phase 3.1 仅保留 data URL 字符串，前端可选择性渲染。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
}

/// MCP server 连接状态。emit 到前端用于 StatusBar 渲染。
///
/// `#[serde(tag = "kind", rename_all = "lowercase")]` ⇒
/// `{ "kind": "connected" }` / `{ "kind": "error", "message": "..." }`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum McpServerState {
    Connecting,
    Connected,
    Error { message: String },
    Disconnected,
}

/// `mcp-server-state` 事件的 payload。
///
/// 字段命名遵循 `ipc-contracts.md`：`#[serde(rename_all = "camelCase")]`，
/// wire 上是 `{ "serverId": "...", "state": {...} }`。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatePayload {
    pub server_id: String,
    pub state: McpServerState,
}

/// 把 MCP server 返回的 tool 转换为 LLM 看到的 `ChatToolDefinition`。
///
/// - tool 名命名空间：`mcp__{server_id}__{tool_name}`（防多 server 冲突）。
/// - `sensitive` 由 `mcp_tool_requires_confirmation` 计算（annotations 三 hint + fallback）。
/// - 空 description → 填 `MCP tool {name}` 兜底。
/// - 空 input_schema → 填 `{ "type": "object", "properties": {} }` 兜底。
pub fn tool_definition_from_mcp(server: &ChatMcpServer, tool: McpTool) -> ChatToolDefinition {
    let name = format!("mcp__{}__{}", server.id, tool.name);
    let sensitive = mcp_tool_requires_confirmation(&tool);
    let description = if tool.description.trim().is_empty() {
        format!("MCP tool {}", tool.name)
    } else {
        tool.description
    };
    let input_schema = if tool.input_schema.is_null() {
        serde_json::json!({ "type": "object", "properties": {} })
    } else {
        tool.input_schema
    };
    ChatToolDefinition {
        name,
        description,
        input_schema,
        source: "mcp".to_string(),
        server_id: Some(server.id.clone()),
        sensitive,
    }
}

/// 决定 MCP tool 是否需要 approval。照搬 Kivio `mcp_tool_requires_confirmation`。
///
/// 优先级（按 MCP 协议 annotations）：
/// 1. `destructiveHint == true` → 需要 approval
/// 2. `openWorldHint == true` → 需要 approval（外部副作用，如发邮件、调外部 API）
/// 3. `readOnlyHint == false` → 需要 approval（明确说不是只读）
/// 4. `readOnlyHint == true` → 不需要 approval
/// 5. 无 annotations → fallback 到 `looks_sensitive_tool(tool.name)` 启发式
pub fn mcp_tool_requires_confirmation(tool: &McpTool) -> bool {
    let annotations = tool.annotations.as_ref();
    if annotation_bool(annotations, "destructiveHint") == Some(true) {
        return true;
    }
    if annotation_bool(annotations, "openWorldHint") == Some(true) {
        return true;
    }
    if annotation_bool(annotations, "readOnlyHint") == Some(false) {
        return true;
    }
    if annotation_bool(annotations, "readOnlyHint") == Some(true) {
        return false;
    }
    looks_sensitive_tool(&tool.name)
}

/// 从 MCP annotations JSON 中取一个 bool 字段。同时支持 camelCase / snake_case key。
fn annotation_bool(annotations: Option<&Value>, key: &str) -> Option<bool> {
    let annotations = annotations?;
    let snake_key = to_snake_case(key);
    annotations
        .get(key)
        .or_else(|| annotations.get(&snake_key))
        .and_then(|v| v.as_bool())
}

fn to_snake_case(value: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

/// 启发式：按 tool 名关键词猜测是否敏感。无 annotations 时使用。
///
/// 照搬 Kivio `looks_sensitive_tool`（`mcp/types.rs:915`）。
pub fn looks_sensitive_tool(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    [
        "write", "delete", "remove", "exec", "shell", "command", "run", "update", "patch", "move",
        "rename", "create", "save", "upload", "publish", "replace", "modify", "edit", "insert",
        "drop", "truncate", "grant", "revoke", "deploy", "apply",
    ]
    .iter()
    .any(|needle| name.contains(needle))
}

/// 把 `tools/call` 的 JSON-RPC response result 解析为 `McpToolCallResult`。
///
/// 输入应当是 JSON-RPC `result` 字段（已 unwrap 出来），形如：
/// ```json
/// { "content": [{ "type": "text", "text": "..." }], "isError": false, "structuredContent": {...} }
/// ```
pub fn parse_tool_result(value: Value) -> McpToolCallResult {
    let is_error = value
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let structured_content = value.get("structuredContent").cloned();

    let mut artifacts: Vec<String> = Vec::new();
    let content = value
        .get("content")
        .and_then(|c| c.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| content_block_text(item, &mut artifacts))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| {
            if artifacts.is_empty() {
                compact_json(&value, 4000)
            } else {
                String::new()
            }
        });

    McpToolCallResult {
        content,
        is_error,
        structured_content,
        artifacts,
    }
}

/// 从单个 content block 提取文本。图片块转 data URL 塞进 artifacts，文本部分返回 placeholder。
fn content_block_text(item: &Value, artifacts: &mut Vec<String>) -> Option<String> {
    let ty = item.get("type").and_then(|v| v.as_str())?;
    match ty {
        "text" => item.get("text").and_then(|v| v.as_str()).map(String::from),
        "image" => {
            let data = item.get("data").and_then(|v| v.as_str())?;
            let mime = item
                .get("mimeType")
                .and_then(|v| v.as_str())
                .unwrap_or("image/png");
            let url = format!("data:{mime};base64,{data}");
            artifacts.push(url);
            Some(format!("[image: {mime}]"))
        }
        _ => None,
    }
}

fn compact_json(value: &Value, max_chars: usize) -> String {
    let s = value.to_string();
    if s.len() <= max_chars {
        s
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

/// 解析 `mcp__{server_id}__{tool_name}` 为 `(server_id, tool_name)`。
///
/// 返回 None 表示不是合法的 MCP tool 名（不以 `mcp__` 开头或格式错）。
pub fn parse_mcp_name(name: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = name.splitn(3, "__").collect();
    if parts.len() != 3 || parts[0] != "mcp" {
        return None;
    }
    let server_id = parts[1];
    let tool_name = parts[2];
    if server_id.is_empty() || tool_name.is_empty() {
        return None;
    }
    Some((server_id.to_string(), tool_name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(id: &str) -> ChatMcpServer {
        ChatMcpServer {
            id: id.into(),
            name: format!("Server {}", id),
            enabled: true,
            transport: "stdio".into(),
            command: "echo".into(),
            args: Vec::new(),
            env: std::collections::HashMap::new(),
            cwd: None,
            enabled_tools: Vec::new(),
            ..Default::default()
        }
    }

    fn mcp_tool(name: &str, annotations: Option<Value>) -> McpTool {
        McpTool {
            name: name.into(),
            description: format!("desc for {}", name),
            input_schema: serde_json::json!({ "type": "object" }),
            annotations,
        }
    }

    // ---- ChatMcpServer / McpTool deserialize ----

    #[test]
    fn mcp_tool_deserialize_minimal() {
        let raw = r#"{ "name": "read_file" }"#;
        let t: McpTool = serde_json::from_str(raw).unwrap();
        assert_eq!(t.name, "read_file");
        assert_eq!(t.description, "");
        assert!(t.input_schema.is_null());
        assert!(t.annotations.is_none());
    }

    #[test]
    fn mcp_tool_deserialize_with_annotations() {
        let raw = r#"{
            "name": "write_file",
            "description": "writes a file",
            "inputSchema": { "type": "object" },
            "annotations": { "destructiveHint": true }
        }"#;
        let t: McpTool = serde_json::from_str(raw).unwrap();
        assert_eq!(t.name, "write_file");
        assert_eq!(t.description, "writes a file");
        assert_eq!(t.input_schema, serde_json::json!({ "type": "object" }));
        assert_eq!(
            t.annotations,
            Some(serde_json::json!({ "destructiveHint": true }))
        );
    }

    // ---- tool_definition_from_mcp ----

    #[test]
    fn tool_definition_from_mcp_basic() {
        let srv = server("fs");
        let tool = mcp_tool("read_file", None);
        let def = tool_definition_from_mcp(&srv, tool);
        assert_eq!(def.name, "mcp__fs__read_file");
        assert_eq!(def.source, "mcp");
        assert_eq!(def.server_id.as_deref(), Some("fs"));
        assert_eq!(def.description, "desc for read_file");
        assert!(!def.sensitive, "无 annotations + read_file 不应敏感");
    }

    #[test]
    fn tool_definition_from_mcp_destructive_hint_sensitive() {
        let srv = server("db");
        let tool = mcp_tool("exec_sql", Some(serde_json::json!({ "destructiveHint": true })));
        let def = tool_definition_from_mcp(&srv, tool);
        assert!(
            def.sensitive,
            "destructiveHint=true 必须敏感"
        );
    }

    #[test]
    fn tool_definition_from_mcp_open_world_hint_sensitive() {
        let srv = server("gh");
        let tool = mcp_tool(
            "send_email",
            Some(serde_json::json!({ "openWorldHint": true })),
        );
        let def = tool_definition_from_mcp(&srv, tool);
        assert!(def.sensitive, "openWorldHint=true 必须敏感");
    }

    #[test]
    fn tool_definition_from_mcp_readonly_hint_not_sensitive() {
        let srv = server("fs");
        let tool = mcp_tool(
            "read_file",
            Some(serde_json::json!({ "readOnlyHint": true })),
        );
        let def = tool_definition_from_mcp(&srv, tool);
        assert!(!def.sensitive, "readOnlyHint=true 必须不敏感");
    }

    #[test]
    fn tool_definition_from_mcp_readonly_false_sensitive() {
        let srv = server("fs");
        let tool = mcp_tool(
            "stat",
            Some(serde_json::json!({ "readOnlyHint": false })),
        );
        let def = tool_definition_from_mcp(&srv, tool);
        assert!(def.sensitive, "readOnlyHint=false 必须敏感");
    }

    #[test]
    fn tool_definition_from_mcp_no_annotations_fallback_sensitive() {
        let srv = server("fs");
        let tool = mcp_tool("write_file", None);
        let def = tool_definition_from_mcp(&srv, tool);
        assert!(
            def.sensitive,
            "无 annotations + write_file 名匹配敏感词 → fallback 敏感"
        );
    }

    #[test]
    fn tool_definition_from_mcp_empty_description_fallback() {
        let srv = server("fs");
        let tool = McpTool {
            name: "weird".into(),
            description: "   ".into(),
            input_schema: serde_json::json!({ "type": "object" }),
            annotations: None,
        };
        let def = tool_definition_from_mcp(&srv, tool);
        assert_eq!(def.description, "MCP tool weird");
    }

    #[test]
    fn tool_definition_from_mcp_null_input_schema_fallback() {
        let srv = server("fs");
        let tool = McpTool {
            name: "weird".into(),
            description: "d".into(),
            input_schema: Value::Null,
            annotations: None,
        };
        let def = tool_definition_from_mcp(&srv, tool);
        assert_eq!(
            def.input_schema,
            serde_json::json!({ "type": "object", "properties": {} })
        );
    }

    #[test]
    fn tool_definition_from_mcp_annotation_snake_case_key() {
        // 部分 server 可能用 snake_case 写 annotations（虽然协议规范是 camelCase）
        let srv = server("fs");
        let tool = mcp_tool(
            "stat",
            Some(serde_json::json!({ "read_only_hint": true })),
        );
        let def = tool_definition_from_mcp(&srv, tool);
        assert!(!def.sensitive, "snake_case read_only_hint=true 也应识别");
    }

    // ---- looks_sensitive_tool ----

    #[test]
    fn looks_sensitive_tool_covers_common_verbs() {
        for name in [
            "write_file",
            "delete_row",
            "exec_command",
            "run_query",
            "save_record",
            "uploadAsset",
            "publish_page",
            "replace_rows",
        ] {
            assert!(looks_sensitive_tool(name), "{name} 应被识别为敏感");
        }
    }

    #[test]
    fn looks_sensitive_tool_skips_readonly_names() {
        for name in ["read_file", "list_dir", "search", "web_search", "get_status"] {
            assert!(!looks_sensitive_tool(name), "{name} 不应被识别为敏感");
        }
    }

    // ---- parse_tool_result ----

    #[test]
    fn parse_tool_result_is_error_true() {
        let v = serde_json::json!({
            "content": [{ "type": "text", "text": "Assertion failed: x > 0" }],
            "isError": true
        });
        let r = parse_tool_result(v);
        assert!(r.is_error);
        assert_eq!(r.content, "Assertion failed: x > 0");
        assert!(r.structured_content.is_none());
        assert!(r.artifacts.is_empty());
    }

    #[test]
    fn parse_tool_result_is_error_false() {
        let v = serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }],
            "isError": false
        });
        let r = parse_tool_result(v);
        assert!(!r.is_error);
        assert_eq!(r.content, "ok");
    }

    #[test]
    fn parse_tool_result_structured_content() {
        let v = serde_json::json!({
            "content": [{ "type": "text", "text": "summary" }],
            "structuredContent": { "items": [{ "title": "A" }] },
            "isError": false
        });
        let r = parse_tool_result(v);
        assert_eq!(r.content, "summary");
        assert_eq!(
            r.structured_content,
            Some(serde_json::json!({ "items": [{ "title": "A" }] }))
        );
    }

    #[test]
    fn parse_tool_result_image_artifact() {
        // "hello" base64 → aGVsbG8=
        let v = serde_json::json!({
            "content": [
                { "type": "text", "text": "here is a chart" },
                { "type": "image", "data": "aGVsbG8=", "mimeType": "image/png" }
            ],
            "isError": false
        });
        let r = parse_tool_result(v);
        assert_eq!(r.artifacts.len(), 1);
        assert!(r.artifacts[0].starts_with("data:image/png;base64,aGVsbG8="));
        assert_eq!(r.content, "here is a chart\n[image: image/png]");
    }

    #[test]
    fn parse_tool_result_multiple_text_blocks_joined() {
        let v = serde_json::json!({
            "content": [
                { "type": "text", "text": "line1" },
                { "type": "text", "text": "line2" },
                { "type": "text", "text": "line3" }
            ],
            "isError": false
        });
        let r = parse_tool_result(v);
        assert_eq!(r.content, "line1\nline2\nline3");
    }

    #[test]
    fn parse_tool_result_empty_content_fallback_to_compact_json() {
        let v = serde_json::json!({
            "isError": false,
            "someField": "value"
        });
        let r = parse_tool_result(v);
        assert!(!r.content.is_empty(), "无 content 数组时应 fallback 到 compact json");
        assert!(r.content.contains("someField"));
    }

    #[test]
    fn parse_tool_result_missing_is_error_defaults_false() {
        let v = serde_json::json!({
            "content": [{ "type": "text", "text": "ok" }]
        });
        let r = parse_tool_result(v);
        assert!(!r.is_error, "缺 isError 字段默认 false");
    }

    // ---- parse_mcp_name ----

    #[test]
    fn parse_mcp_name_valid() {
        let (sid, tn) = parse_mcp_name("mcp__fs__read_file").unwrap();
        assert_eq!(sid, "fs");
        assert_eq!(tn, "read_file");
    }

    #[test]
    fn parse_mcp_name_with_dashes_in_server_id() {
        let (sid, tn) = parse_mcp_name("mcp__my-server__tool").unwrap();
        assert_eq!(sid, "my-server");
        assert_eq!(tn, "tool");
    }

    #[test]
    fn parse_mcp_name_invalid_no_prefix() {
        assert!(parse_mcp_name("read_file").is_none());
        assert!(parse_mcp_name("native__fs__tool").is_none());
    }

    #[test]
    fn parse_mcp_name_invalid_empty_components() {
        assert!(parse_mcp_name("mcp____tool").is_none(), "空 server_id 应失败");
        assert!(parse_mcp_name("mcp__fs__").is_none(), "空 tool_name 应失败");
    }

    #[test]
    fn parse_mcp_name_tool_name_with_double_underscore() {
        // splitn(3, "__") 保证 tool_name 中后续的 __ 保留
        let (sid, tn) = parse_mcp_name("mcp__fs__weird__name").unwrap();
        assert_eq!(sid, "fs");
        assert_eq!(tn, "weird__name");
    }

    // ---- McpServerState serialization ----

    #[test]
    fn mcp_server_state_serialization() {
        let s = McpServerState::Connected;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "connected" }));

        let s = McpServerState::Error { message: "boom".into() };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "error", "message": "boom" }));

        let s = McpServerState::Connecting;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "connecting" }));

        let s = McpServerState::Disconnected;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v, serde_json::json!({ "kind": "disconnected" }));
    }

    #[test]
    fn mcp_server_state_payload_camel_case() {
        let p = McpServerStatePayload {
            server_id: "fs".into(),
            state: McpServerState::Connected,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["serverId"], "fs");
        assert_eq!(v["state"]["kind"], "connected");
        // snake_case 不应出现在 wire 上
        assert!(v.get("server_id").is_none());
    }
}

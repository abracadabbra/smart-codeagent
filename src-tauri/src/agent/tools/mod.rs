//! 工具系统公共类型 + 子模块声明。

pub mod ask_user;
pub mod background;
pub mod bash;
pub mod deny_list;
pub mod edit;
pub mod glob;
pub mod grep;
pub mod ls;
pub mod path;
pub mod read;
pub mod write;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// 工具执行上下文：loop 注入，前端不可控。
///
/// 借 Kivio `execute.rs:34-54` 的 `ToolExecutionContext` 形态，砍掉了 sub-agent 字段。
///
/// Phase 3.2：加 `conversation_id` 字段，用于 host trait 的 per-conv 路由
/// （`request_tool_approval` / `request_ask_user` 从 ctx 读 conv_id）。
#[derive(Debug, Clone)]
pub struct ToolContext {
    pub conversation_id: String,
    pub run_id: String,
    pub message_id: String,
    pub tool_call_id: String,
    pub round: u32,
    pub generation: u64,
}

/// 工具执行输出（成功路径）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    /// 给 LLM 看的内容（Anthropic `tool_result.content`）
    pub content: String,
    /// 给前端用的结构化字段（ToolResultCard 解析用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    /// 文件产物路径列表（Write / Edit 填，前端 PreviewPane 跳到文件用）
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// 是否截断（Read 大文件时设 true）
    #[serde(default)]
    pub truncated: bool,
}

/// 工具执行错误（失败路径）。
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("invalid arguments: {0}")]
    InvalidArgs(String),
    #[error("path resolution failed: {0}")]
    Path(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("permission denied: {0}")]
    Permission(String),
    #[error("command blocked by safety policy: {0}")]
    Denied(String),
    #[error("execution failed: {0}")]
    Execution(String),
    #[error("timeout after {0}ms")]
    Timeout(u64),
    #[error("cancelled by user")]
    Cancelled,
    #[error("not implemented: {0}")]
    NotImplemented(String),
}

pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send + 'a>>;

/// 工具 trait。所有 native 工具都实现这个。
///
/// 设计参考 Kivio MCP 工具的注册形态，但 trait 抽象直接做在工具侧（Kivio 是分开的
/// ChatToolDefinition + 内部 tool 映射）。
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON Schema 描述输入参数（Anthropic `tools[].input_schema`）
    fn input_schema(&self) -> serde_json::Value;

    /// 工具是否敏感（如 ask_user 算敏感，因为会阻塞等用户）。
    /// 敏感工具默认走 approval 流。
    fn is_sensitive(&self) -> bool {
        false
    }

    /// 工具是否破坏性（Write / Edit / Bash / Kill）。破坏性工具走 approval 流。
    fn is_destructive(&self) -> bool {
        false
    }

    /// 实际执行。`ctx` 由 loop 注入，`args` 是 LLM 输出的 JSON。
    fn execute<'a>(&'a self, args: serde_json::Value, ctx: &'a ToolContext) -> ToolFuture<'a>;
}

/// LLM 看到的工具定义（Anthropic `tools` 字段的 wire format）。
///
/// 字段保持与 Kivio ChatToolDefinition 同形（name/description/input_schema/source/server_id/sensitive），
/// Phase 4 加 MCP 时 source="mcp" + server_id 启用。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    #[serde(default = "default_source")]
    pub source: String, // "native" 固定（Phase 2 简化）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>, // None for native
    #[serde(default)]
    pub sensitive: bool,
}

fn default_source() -> String {
    "native".to_string()
}

/// 工具注册表。启动时构造，所有工具 `register` 后冻结（`Arc<dyn Tool>` 不允许修改）。
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register<T: Tool + 'static>(mut self, tool: T) -> Self {
        self.tools.push(Arc::new(tool));
        self
    }

    pub fn register_boxed(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// 按名字查找工具。
    pub fn by_name(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    /// 给 LLM 的工具定义列表（Anthropic `tools` 字段）。
    pub fn definitions(&self) -> Vec<ChatToolDefinition> {
        self.tools
            .iter()
            .map(|t| ChatToolDefinition {
                name: t.name().to_string(),
                description: t.description().to_string(),
                input_schema: t.input_schema(),
                source: "native".to_string(),
                server_id: None,
                sensitive: t.is_sensitive() || t.is_destructive(),
            })
            .collect()
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 工具调用记录（前端 + 后端共用，给 ToolCallCard 渲染）。
///
/// 借 Kivio `chat/types.rs:203` 的 `ToolCallRecord`，砍掉了 trace_id / span_id
/// （无 OpenTelemetry 集成）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,
    pub name: String,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    /// 原始 JSON 参数字符串（保留格式，前端 syntax highlight）
    pub arguments: String,
    pub status: ToolCallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    pub round: u32,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<serde_json::Value>,
}

/// 工具调用生命周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ToolCallStatus {
    /// 准备中（解析参数 + 等待 approval）
    Pending,
    /// 正在执行
    Running,
    /// 成功
    Success,
    /// 失败（工具返回 Err）
    Error,
    /// 用户取消（拒绝 approval 或 cancel 命令）
    Cancelled,
    /// 跳过（如敏感但用户标记"以后都允许"——Phase 2 不实现，留字段）
    Skipped,
}

/// AskUser 工具专用类型。
///
/// 借 Kivio `chat/ask_user.rs:27-67` 的形态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AskUserOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AskUserQuestion {
    pub id: String,
    pub prompt: String,
    pub options: Vec<AskUserOption>,
    #[serde(default)]
    pub allow_multiple: bool,
    #[serde(default)]
    pub allow_custom: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AskUserPromptPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub questions: Vec<AskUserQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AskUserAnswer {
    #[serde(default, alias = "selectedOptionIds")]
    pub selected_option_ids: Vec<String>,
    #[serde(default, alias = "customText")]
    pub custom_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct AskUserResponseResult {
    /// "answered" | "skipped" | "timeout" | "cancelled"
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub answers: HashMap<String, AskUserAnswer>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct EchoTool;
    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "echoes input"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "msg": { "type": "string" } },
                "required": ["msg"],
            })
        }
        fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> ToolFuture<'_> {
            Box::pin(async move {
                let msg = args
                    .get("msg")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidArgs("missing msg".into()))?;
                Ok(ToolOutput {
                    content: msg.to_string(),
                    structured: None,
                    artifacts: vec![],
                    truncated: false,
                })
            })
        }
    }

    struct DestructiveTool;
    #[async_trait]
    impl Tool for DestructiveTool {
        fn name(&self) -> &'static str {
            "destroy"
        }
        fn description(&self) -> &'static str {
            "destroys things"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object"})
        }
        fn is_destructive(&self) -> bool {
            true
        }
        fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolFuture<'_> {
            Box::pin(async move {
                Ok(ToolOutput {
                    content: "ok".into(),
                    structured: None,
                    artifacts: vec![],
                    truncated: false,
                })
            })
        }
    }

    #[test]
    fn registry_register_and_lookup() {
        let reg = ToolRegistry::new()
            .register(EchoTool)
            .register(DestructiveTool);
        assert_eq!(reg.len(), 2);
        assert!(reg.by_name("echo").is_some());
        assert!(reg.by_name("destroy").is_some());
        assert!(reg.by_name("nope").is_none());
    }

    #[test]
    fn registry_definitions_camel_case() {
        let reg = ToolRegistry::new().register(DestructiveTool);
        let defs = reg.definitions();
        let json = serde_json::to_value(&defs[0]).unwrap();
        assert_eq!(json["name"], "destroy");
        assert_eq!(json["source"], "native");
        assert_eq!(json["sensitive"], true, "destructive implies sensitive");
        assert!(json.get("serverId").is_none() || json["serverId"].is_null());
        assert!(json.get("inputSchema").is_some());
    }

    #[test]
    fn tool_call_record_serialization() {
        let rec = ToolCallRecord {
            id: "tc_1".into(),
            name: "echo".into(),
            source: "native".into(),
            server_id: None,
            arguments: r#"{"msg":"hi"}"#.into(),
            status: ToolCallStatus::Success,
            result_preview: Some("hi".into()),
            error: None,
            duration_ms: Some(42),
            started_at: Some(1),
            completed_at: Some(2),
            round: 1,
            sensitive: false,
            artifacts: vec![],
            structured_content: None,
        };
        let json = serde_json::to_value(&rec).unwrap();
        assert_eq!(json["id"], "tc_1");
        assert_eq!(json["status"], "Success");
        assert_eq!(json["durationMs"], 42);
        assert_eq!(json["startedAt"], 1);
        assert!(json.get("server_id").is_none(), "snake_case leaks");
    }

    #[test]
    fn ask_user_payload_round_trip() {
        let payload = AskUserPromptPayload {
            title: Some("Pick a runtime".into()),
            questions: vec![AskUserQuestion {
                id: "q1".into(),
                prompt: "Which runtime?".into(),
                options: vec![
                    AskUserOption {
                        id: "node".into(),
                        label: "Node.js".into(),
                        description: None,
                    },
                    AskUserOption {
                        id: "deno".into(),
                        label: "Deno".into(),
                        description: Some("TypeScript native".into()),
                    },
                ],
                allow_multiple: false,
                allow_custom: true,
            }],
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            json["questions"][0]["options"][1]["description"],
            "TypeScript native"
        );
        // round trip
        let s = serde_json::to_string(&payload).unwrap();
        let back: AskUserPromptPayload = serde_json::from_str(&s).unwrap();
        assert_eq!(back, payload);
    }
}

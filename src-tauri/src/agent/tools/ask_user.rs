//! `ask_user` 工具：让 LLM 在对话中向用户提问（多选/单选 + 自定义输入）。
//!
//! 借 Kivio `chat/ask_user.rs:27-67` 的 `AskUserPromptPayload` 等结构，
//! 加上 `execute_ask_user_call` 的阻塞等待 oneshot 模式。
//!
//! Phase 2 stub：execute 直接返回 `ToolError::NotImplemented`，
//! 真正接入 oneshot 桥接放在 Round 3（AgentHost）。

use async_trait::async_trait;
use serde::Deserialize;

use super::{
    AskUserPromptPayload, AskUserQuestion, AskUserResponseResult, Tool, ToolContext, ToolError,
    ToolFuture,
};

pub struct AskUserTool;

#[derive(Debug, Deserialize)]
struct AskUserArgs {
    title: Option<String>,
    questions: Vec<AskUserQuestion>,
}

#[async_trait]
impl Tool for AskUserTool {
    fn name(&self) -> &'static str {
        "ask_user"
    }

    fn description(&self) -> &'static str {
        "Ask the user one or more multiple-choice questions during a task. \
         Each question can be single-select, multi-select (allow_multiple=true), \
         and/or allow a custom text answer (allow_custom=true). \
         Blocks until the user answers. Sensitive (always requires approval flow)."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Optional dialog title." },
                "questions": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": { "type": "string" },
                            "prompt": { "type": "string" },
                            "options": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "id": { "type": "string" },
                                        "label": { "type": "string" },
                                        "description": { "type": "string" }
                                    },
                                    "required": ["id", "label"]
                                }
                            },
                            "allow_multiple": { "type": "boolean", "default": false },
                            "allow_custom": { "type": "boolean", "default": false }
                        },
                        "required": ["id", "prompt", "options"]
                    }
                }
            },
            "required": ["questions"]
        })
    }

    fn is_sensitive(&self) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: serde_json::Value, _ctx: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: AskUserArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("ask_user: {e}")))?;

            if args.questions.is_empty() {
                return Err(ToolError::InvalidArgs(
                    "ask_user: questions must be non-empty".into(),
                ));
            }
            for q in &args.questions {
                if q.options.is_empty() {
                    return Err(ToolError::InvalidArgs(format!(
                        "ask_user: question '{}' has no options",
                        q.id
                    )));
                }
            }

            // Round 3 接 AgentHost；这里 stub 报错让 round 单测能编译过
            Err(ToolError::NotImplemented(
                "ask_user requires AgentHost integration (Round 3)".into(),
            ))
        })
    }
}

/// 把 `AskUserResponseResult` 格式化成给 LLM 的 tool_result 文本。
pub fn format_ask_user_response(
    payload: &AskUserPromptPayload,
    response: &AskUserResponseResult,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("phase: {}", response.phase));
    for q in &payload.questions {
        match response.answers.get(&q.id) {
            Some(ans) => {
                let mut pieces = Vec::new();
                if !ans.selected_option_ids.is_empty() {
                    let labels: Vec<String> = q
                        .options
                        .iter()
                        .filter(|o| ans.selected_option_ids.contains(&o.id))
                        .map(|o| o.label.clone())
                        .collect();
                    pieces.push(format!("selected: {}", labels.join(", ")));
                }
                if let Some(text) = &ans.custom_text {
                    if !text.is_empty() {
                        pieces.push(format!("custom: {text}"));
                    }
                }
                parts.push(format!("{} → {}", q.id, pieces.join("; ")));
            }
            None => parts.push(format!("{} → (skipped)", q.id)),
        }
    }
    parts.join("\n")
}

/// 把 `AskUserResponseResult` 转成 structured_content（前端解析用）。
pub fn ask_user_structured_content(response: &AskUserResponseResult) -> serde_json::Value {
    serde_json::json!({
        "phase": response.phase,
        "answers": response.answers,
    })
}

/// 构造空的 skipped response（用户点"跳过"）。
pub fn skipped_response() -> AskUserResponseResult {
    AskUserResponseResult {
        phase: "skipped".into(),
        answers: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::tools::AskUserAnswer;
    use std::collections::HashMap;

    fn ctx() -> ToolContext {
        ToolContext {
            conversation_id: "t".into(),
            run_id: "t".into(),
            message_id: "t".into(),
            tool_call_id: "tc_test".into(),
            round: 0,
            generation: 0,
        }
    }

    #[tokio::test]
    async fn not_implemented_for_now() {
        let tool = AskUserTool;
        let res = tool
            .execute(
                serde_json::json!({
                    "questions": [{
                        "id": "q1",
                        "prompt": "Pick one",
                        "options": [{ "id": "a", "label": "A" }]
                    }]
                }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::NotImplemented(_))));
    }

    #[tokio::test]
    async fn empty_questions_errors() {
        let tool = AskUserTool;
        let res = tool
            .execute(serde_json::json!({ "questions": [] }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::InvalidArgs(_))));
    }

    #[tokio::test]
    async fn question_without_options_errors() {
        let tool = AskUserTool;
        let res = tool
            .execute(
                serde_json::json!({
                    "questions": [{
                        "id": "q1",
                        "prompt": "Pick",
                        "options": []
                    }]
                }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::InvalidArgs(_))));
    }

    #[test]
    fn format_response_text() {
        let payload = AskUserPromptPayload {
            title: Some("test".into()),
            questions: vec![AskUserQuestion {
                id: "q1".into(),
                prompt: "Pick".into(),
                options: vec![
                    super::super::AskUserOption {
                        id: "a".into(),
                        label: "Apple".into(),
                        description: None,
                    },
                    super::super::AskUserOption {
                        id: "b".into(),
                        label: "Banana".into(),
                        description: None,
                    },
                ],
                allow_multiple: false,
                allow_custom: false,
            }],
        };
        let mut answers = HashMap::new();
        answers.insert(
            "q1".into(),
            AskUserAnswer {
                selected_option_ids: vec!["a".into()],
                custom_text: None,
            },
        );
        let response = AskUserResponseResult {
            phase: "answered".into(),
            answers,
        };
        let text = format_ask_user_response(&payload, &response);
        assert!(text.contains("Apple"));
    }
}

//! `edit_file` 工具：精确文本替换。`old_text` 必须唯一匹配，否则报错。
//!
//! 借 Kivio `native_tools/files.rs:301` 的 `edit_file` 形态，简化了 search/replace 协议。

use async_trait::async_trait;
use serde::Deserialize;

use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub struct EditTool;

#[derive(Debug, Deserialize)]
struct EditArgs {
    path: String,
    old_text: String,
    new_text: String,
    #[serde(default)]
    replace_all: bool,
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit_file"
    }

    fn description(&self) -> &'static str {
        "Replace exact text in a file. By default, old_text must occur exactly once. \
         Set replace_all=true to replace every occurrence. Requires user approval."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_text": { "type": "string", "description": "Exact text to find." },
                "new_text": { "type": "string", "description": "Replacement text." },
                "replace_all": {
                    "type": "boolean",
                    "default": false,
                    "description": "Replace every occurrence instead of requiring uniqueness."
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: serde_json::Value, _ctx: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: EditArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("edit_file: {e}")))?;
            let resolved = super::path::resolve_tool_path(&args.path)
                .map_err(|e| ToolError::Path(format!("edit_file: {e}")))?;

            let original = std::fs::read_to_string(&resolved).map_err(ToolError::Io)?;
            let count = original.matches(&args.old_text).count();

            if count == 0 {
                return Err(ToolError::Execution(format!(
                    "edit_file: old_text not found in {}",
                    resolved.display()
                )));
            }

            if count > 1 && !args.replace_all {
                return Err(ToolError::Execution(format!(
                    "edit_file: old_text occurs {count} times in {}; \
                     pass replace_all=true or narrow old_text",
                    resolved.display()
                )));
            }

            let updated = if args.replace_all {
                original.replace(&args.old_text, &args.new_text)
            } else {
                original.replacen(&args.old_text, &args.new_text, 1)
            };

            std::fs::write(&resolved, updated.as_bytes()).map_err(ToolError::Io)?;

            Ok(ToolOutput {
                content: format!(
                    "replaced {} occurrence(s) in {}",
                    if args.replace_all { count } else { 1 },
                    resolved.display()
                ),
                structured: Some(serde_json::json!({
                    "replacements": if args.replace_all { count } else { 1 },
                    "file": resolved.to_string_lossy().to_string(),
                    "oldContent": original,
                    "newContent": updated,
                })),
                artifacts: vec![resolved.to_string_lossy().to_string()],
                truncated: false,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ToolContext {
        ToolContext {
            conversation_id: "test".into(),
            run_id: "test".into(),
            message_id: "test".into(),
            tool_call_id: "tc_test".into(),
            round: 0,
            generation: 0,
        }
    }

    #[tokio::test]
    async fn replaces_unique_match() {
        let tmp = std::env::temp_dir().join(format!("edit_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "hello world\nbye world").unwrap();

        let tool = EditTool;
        let out = tool
            .execute(
                serde_json::json!({
                    "path": tmp.to_string_lossy(),
                    "old_text": "hello",
                    "new_text": "hi"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(out.structured.unwrap()["replacements"], 1);
        assert_eq!(
            std::fs::read_to_string(&tmp).unwrap(),
            "hi world\nbye world"
        );

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn rejects_ambiguous_match_without_replace_all() {
        let tmp = std::env::temp_dir().join(format!("edit_amb_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "foo bar foo").unwrap();

        let tool = EditTool;
        let res = tool
            .execute(
                serde_json::json!({
                    "path": tmp.to_string_lossy(),
                    "old_text": "foo",
                    "new_text": "baz"
                }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::Execution(_))));

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn replace_all_when_ambiguous() {
        let tmp = std::env::temp_dir().join(format!("edit_all_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "foo bar foo").unwrap();

        let tool = EditTool;
        tool.execute(
            serde_json::json!({
                "path": tmp.to_string_lossy(),
                "old_text": "foo",
                "new_text": "baz",
                "replace_all": true
            }),
            &ctx(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "baz bar baz");

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn missing_old_text_errors() {
        let tmp = std::env::temp_dir().join(format!("edit_nf_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "abc").unwrap();

        let tool = EditTool;
        let res = tool
            .execute(
                serde_json::json!({
                    "path": tmp.to_string_lossy(),
                    "old_text": "xyz",
                    "new_text": "123"
                }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::Execution(_))));

        std::fs::remove_file(&tmp).ok();
    }
}

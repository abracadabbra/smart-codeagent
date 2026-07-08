//! `write_file` 工具：覆盖写文件。父目录自动创建。
//!
//! 借 Kivio `native_tools/files.rs:255` 的 `write_file` 形态。

use async_trait::async_trait;
use serde::Deserialize;

use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub struct WriteTool;

#[derive(Debug, Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write_file"
    }

    fn description(&self) -> &'static str {
        "Write content to a file, overwriting any existing content. \
         Parent directories are created automatically. Requires user approval."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to write to. Relative paths resolve against cwd."
                },
                "content": {
                    "type": "string",
                    "description": "UTF-8 content to write."
                }
            },
            "required": ["path", "content"]
        })
    }

    fn is_destructive(&self) -> bool {
        true
    }

    fn execute<'a>(&'a self, args: serde_json::Value, _ctx: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: WriteArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("write_file: {e}")))?;
            let resolved = super::path::resolve_tool_path(&args.path)
                .map_err(|e| ToolError::Path(format!("write_file: {e}")))?;

            if let Some(parent) = resolved.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(ToolError::Io)?;
                }
            }

            let bytes = args.content.as_bytes();

            let old_content = std::fs::read_to_string(&resolved).ok();

            std::fs::write(&resolved, bytes).map_err(ToolError::Io)?;

            Ok(ToolOutput {
                content: format!("wrote {} bytes to {}", bytes.len(), resolved.display()),
                structured: Some(serde_json::json!({
                    "bytes_written": bytes.len(),
                    "file": resolved.to_string_lossy().to_string(),
                    "oldContent": old_content,
                    "newContent": args.content,
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
    async fn writes_new_file() {
        let tmp = std::env::temp_dir().join(format!("write_{}.txt", uuid::Uuid::new_v4()));
        let tool = WriteTool;
        let out = tool
            .execute(
                serde_json::json!({
                    "path": tmp.to_string_lossy(),
                    "content": "hello"
                }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(out.structured.unwrap()["bytes_written"], 5);
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "hello");

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn overwrites_existing() {
        let tmp = std::env::temp_dir().join(format!("write_over_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "old").unwrap();

        let tool = WriteTool;
        tool.execute(
            serde_json::json!({ "path": tmp.to_string_lossy(), "content": "new" }),
            &ctx(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&tmp).unwrap(), "new");

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn creates_parent_dirs() {
        let tmp = std::env::temp_dir()
            .join(format!("write_parent_{}", uuid::Uuid::new_v4()))
            .join("nested")
            .join("file.txt");

        let tool = WriteTool;
        tool.execute(
            serde_json::json!({ "path": tmp.to_string_lossy(), "content": "ok" }),
            &ctx(),
        )
        .await
        .unwrap();
        assert!(tmp.exists());

        std::fs::remove_dir_all(tmp.parent().unwrap().parent().unwrap()).ok();
    }

    #[tokio::test]
    async fn missing_path_arg_errors() {
        let tool = WriteTool;
        let res = tool
            .execute(serde_json::json!({ "content": "x" }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::InvalidArgs(_))));
    }
}

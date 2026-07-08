//! `read_file` 工具：读文件文本（UTF-8），最大 2 MB。
//!
//! 借 Kivio `native_tools/files.rs:108` 的 `read_file` 形态，砍掉了 workspace / state 依赖。

use async_trait::async_trait;
use serde::Deserialize;

use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub const MAX_READ_BYTES: u64 = 2 * 1024 * 1024; // 2 MB

pub struct ReadTool;

#[derive(Debug, Deserialize)]
struct ReadArgs {
    path: String,
    #[serde(default)]
    max_bytes: Option<u64>,
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read_file"
    }

    fn description(&self) -> &'static str {
        "Read the contents of a UTF-8 text file. Returns up to max_bytes (default 2 MB). \
         For large files, returns a preview and sets truncated=true."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file. Relative paths resolve against the current working directory."
                },
                "max_bytes": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum number of bytes to read. Defaults to 2 MB."
                }
            },
            "required": ["path"]
        })
    }

    fn execute<'a>(&'a self, args: serde_json::Value, _ctx: &'a ToolContext) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: ReadArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("read_file: {e}")))?;
            let resolved = super::path::resolve_tool_path(&args.path)
                .map_err(|e| ToolError::Path(format!("read_file: {e}")))?;
            let max = args.max_bytes.unwrap_or(MAX_READ_BYTES);

            let metadata = std::fs::metadata(&resolved).map_err(|e| {
                ToolError::Io(std::io::Error::new(
                    e.kind(),
                    format!("{}: {e}", resolved.display()),
                ))
            })?;
            let total = metadata.len();

            // 二进制检测：前 8 KB 包含 NUL → 拒绝
            let head_bytes = std::fs::read(&resolved).map_err(|e| {
                ToolError::Io(std::io::Error::new(
                    e.kind(),
                    format!("{}: {e}", resolved.display()),
                ))
            })?;
            if head_bytes.iter().take(8192).any(|&b| b == 0) {
                return Err(ToolError::Execution(format!(
                    "read_file: {} appears to be binary (contains NUL bytes)",
                    resolved.display()
                )));
            }

            let truncated = total > max;
            let content_bytes = if truncated {
                &head_bytes[..max as usize]
            } else {
                &head_bytes[..]
            };
            let content = String::from_utf8_lossy(content_bytes).to_string();

            Ok(ToolOutput {
                content,
                structured: Some(serde_json::json!({
                    "total_bytes": total,
                    "truncated": truncated,
                })),
                artifacts: vec![resolved.to_string_lossy().to_string()],
                truncated,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
    async fn reads_utf8_file() {
        let tmp = std::env::temp_dir().join(format!("read_test_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "hello world").unwrap();

        let tool = ReadTool;
        let out = tool
            .execute(serde_json::json!({ "path": tmp.to_string_lossy() }), &ctx())
            .await
            .unwrap();
        assert_eq!(out.content, "hello world");
        assert_eq!(out.structured.unwrap()["total_bytes"], 11);
        assert!(!out.truncated);

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn rejects_binary_file() {
        let tmp = std::env::temp_dir().join(format!("read_bin_{}.bin", uuid::Uuid::new_v4()));
        let mut f = std::fs::File::create(&tmp).unwrap();
        f.write_all(&[0x00, 0x01, 0x02, 0xff]).unwrap();

        let tool = ReadTool;
        let res = tool
            .execute(serde_json::json!({ "path": tmp.to_string_lossy() }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::Execution(_))));

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn truncates_large_file() {
        let tmp = std::env::temp_dir().join(format!("read_big_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&tmp, "x".repeat(1000)).unwrap();

        let tool = ReadTool;
        let out = tool
            .execute(
                serde_json::json!({ "path": tmp.to_string_lossy(), "max_bytes": 100 }),
                &ctx(),
            )
            .await
            .unwrap();
        assert_eq!(out.content.len(), 100);
        assert!(out.truncated);

        std::fs::remove_file(&tmp).ok();
    }

    #[tokio::test]
    async fn missing_file_errors() {
        let tool = ReadTool;
        let res = tool
            .execute(
                serde_json::json!({ "path": "/nonexistent_xyz_42.txt" }),
                &ctx(),
            )
            .await;
        assert!(matches!(res, Err(ToolError::Io(_))));
    }

    #[tokio::test]
    async fn missing_path_arg_errors() {
        let tool = ReadTool;
        let res = tool.execute(serde_json::json!({}), &ctx()).await;
        assert!(matches!(res, Err(ToolError::InvalidArgs(_))));
    }
}

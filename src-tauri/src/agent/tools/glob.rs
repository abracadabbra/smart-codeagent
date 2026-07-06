//! `glob_files` 工具：按 glob 模式列出匹配的文件路径。
//!
//! 借 Kivio `native_tools/files.rs:1074` 的 `glob_files` 形态。

use async_trait::async_trait;
use glob::glob;
use serde::Deserialize;

use super::{Tool, ToolContext, ToolError, ToolFuture, ToolOutput};

pub struct GlobTool;

#[derive(Debug, Deserialize)]
struct GlobArgs {
    pattern: String,
    #[serde(default)]
    cwd: Option<String>,
}

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str {
        "glob_files"
    }

    fn description(&self) -> &'static str {
        "List file paths matching a glob pattern (e.g. 'src/**/*.rs'). \
         Pattern is relative to cwd unless 'cwd' is given."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern. Supports ** for recursive and ? for single char."
                },
                "cwd": {
                    "type": "string",
                    "description": "Base directory. Defaults to project root."
                }
            },
            "required": ["pattern"]
        })
    }

    fn execute<'a>(
        &'a self,
        args: serde_json::Value,
        _ctx: &'a ToolContext,
    ) -> ToolFuture<'a> {
        Box::pin(async move {
            let args: GlobArgs = serde_json::from_value(args)
                .map_err(|e| ToolError::InvalidArgs(format!("glob_files: {e}")))?;

            let pattern = match args.cwd {
                Some(cwd) => {
                    let base = super::path::resolve_tool_path(&cwd)
                        .map_err(|e| ToolError::Path(format!("glob_files cwd: {e}")))?;
                    base.join(&args.pattern).to_string_lossy().to_string()
                }
                None => args.pattern.clone(),
            };

            let paths: Vec<String> = match glob(&pattern) {
                Ok(iter) => iter
                    .filter_map(|p| p.ok())
                    .map(|p| p.to_string_lossy().to_string())
                    .collect(),
                Err(e) => return Err(ToolError::InvalidArgs(format!("glob_files: bad pattern: {e}"))),
            };

            let total = paths.len();
            let text = if paths.is_empty() {
                "(no matches)".to_string()
            } else {
                paths.join("\n")
            };

            Ok(ToolOutput {
                content: text,
                structured: Some(serde_json::json!({
                    "paths": paths,
                    "total": total,
                })),
                artifacts: vec![],
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
    async fn finds_matching_files() {
        let dir = std::env::temp_dir().join(format!("glob_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "").unwrap();
        std::fs::write(dir.join("b.rs"), "").unwrap();
        std::fs::write(dir.join("c.txt"), "").unwrap();

        let tool = GlobTool;
        let out = tool
            .execute(
                serde_json::json!({
                    "pattern": format!("{}/*.rs", dir.to_string_lossy()),
                }),
                &ctx(),
            )
            .await
            .unwrap();
        let s = out.structured.unwrap();
        let paths = s["paths"].as_array().unwrap();
        assert_eq!(paths.len(), 2);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn no_match_returns_empty() {
        let tool = GlobTool;
        let out = tool
            .execute(serde_json::json!({ "pattern": "/nonexistent_xyz_42/*.nope" }), &ctx())
            .await
            .unwrap();
        assert_eq!(out.structured.unwrap()["total"], 0);
        assert!(out.content.contains("no matches"));
    }

    #[tokio::test]
    async fn invalid_pattern_errors() {
        let tool = GlobTool;
        let res = tool
            .execute(serde_json::json!({ "pattern": "[unclosed" }), &ctx())
            .await;
        assert!(matches!(res, Err(ToolError::InvalidArgs(_))));
    }
}